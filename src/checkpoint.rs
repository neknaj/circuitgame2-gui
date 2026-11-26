//! Core の確定状態を保存し、回路に対応するファイルから再開する。

use circuitgame2_core::netlist::{
    CheckpointFileError, CheckpointHeader, VmCheckpointFileError, checkpoint_file_len,
    decode_vm_checkpoint, encode_vm_checkpoint,
};

use crate::{Simulation, bit_text};

impl Simulation {
    /// 直近の確定 tick を返す。リセット直後は0。
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    /// 直近の確定 tick に適用した入力をポート宣言順で借りる。
    pub fn applied_input(&self) -> &[bool] {
        &self.last_input
    }

    /// 現在の回路で受理できるスナップショットのバイト数を求める。
    ///
    /// # Errors
    ///
    /// 配線と記憶部をファイル形式で表せない場合に失敗する。
    pub fn snapshot_len(&self) -> Result<usize, String> {
        checkpoint_file_len(
            &self.compiled.vm,
            self.compiled.inputs.ranges(),
            self.compiled.outputs.ranges(),
        )
        .map_err(snapshot_error)
    }

    /// 直近の確定状態と入力を Core 共通形式で保存する。その間、回路は進めない。
    ///
    /// # Errors
    ///
    /// 配線と記憶部を表せない場合や、領域を確保できない場合に失敗する。
    pub fn create_snapshot(&self) -> Result<Vec<u8>, String> {
        let header = CheckpointHeader {
            tick: self.tick,
            generation: self.generation,
            period_numerator: self.period.0,
            period_denominator: self.period.1,
        };
        encode_vm_checkpoint(
            &self.compiled.vm,
            self.compiled.inputs.ranges(),
            self.compiled.outputs.ranges(),
            header,
            &self.last_input,
        )
        .map_err(snapshot_error)
    }

    /// ファイルを照合して確定状態を復元し、その tick の公開出力を返す。
    ///
    /// # Errors
    ///
    /// ファイル、回路の配線、または設定の tick 周期が一致しない場合には現在の回路を保つ。
    pub fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<String, String> {
        let saved = decode_vm_checkpoint(
            bytes,
            &self.compiled.vm,
            self.compiled.inputs.ranges(),
            self.compiled.outputs.ranges(),
            self.period,
        )
        .map_err(snapshot_error)?;
        self.compiled
            .vm
            .restore_checkpoint(&saved.vm)
            .map_err(|error| format!("回路の状態を復元できない。{error:?}"))?;
        let output = self
            .compiled
            .vm
            .observe_current(&saved.input)
            .map_err(|error| format!("確定状態から出力を求められない。{error:?}"))?;
        self.tick = saved.header.tick;
        self.generation = saved.header.generation;
        self.last_input = saved.input;
        Ok(bit_text(output))
    }
}

fn snapshot_error(error: VmCheckpointFileError) -> String {
    let reason = match error {
        VmCheckpointFileError::File(error) => match error {
            CheckpointFileError::ShortHeader => "ファイルのヘッダーが短すぎる。",
            CheckpointFileError::Format => "ファイルの識別子または形式の版が異なる。",
            CheckpointFileError::Period => "ファイルの tick 周期が規定の範囲外である。",
            CheckpointFileError::Length => "ファイルのバイト数が現在の回路と一致しない。",
            CheckpointFileError::Layout => "ファイルと現在の回路で配線が異なる。",
            CheckpointFileError::Allocation => "ファイルを扱う領域を確保できない。",
        },
        VmCheckpointFileError::PeriodMismatch => "ファイルの tick 周期が実行設定と異なる。",
        VmCheckpointFileError::PortLayout => "ファイルの端子配置が回路と異なる。",
        VmCheckpointFileError::MalformedState => "ファイルの状態が規定の形式に合わない。",
        VmCheckpointFileError::Allocation => "保存・復元用の領域を確保できない。",
    };
    format!("スナップショットを処理できない。{reason}")
}

#[cfg(test)]
mod tests {
    use crate::ProjectSettings;

    const RAM: &str = "(using nor)
        (memory bank (address 1) (data (bits 2)))
        (module main
          (in address (data (bits 2)) write-enable)
          (out (read-data (bits 2)))
          (def read-data (bank address data write-enable)))";

    fn ram_settings(period: u64) -> ProjectSettings {
        ProjectSettings::new(format!(
            "sources = ['main.ncg2']\ntick-ns = {{ numerator = {period}, denominator = 1 }}\n[[vram]]\nname = 'screen'\nread-output = 'read-data'\n"
        ).as_bytes()).unwrap()
    }

    #[test]
    fn restores_last_applied_input_and_read_first_ram_at_the_saved_tick() {
        let sources = [RAM.to_owned()];
        let mut recording = ram_settings(10).compile_texts(&sources).unwrap();
        assert_eq!(recording.tick("0101").unwrap(), "00");
        let snapshot = recording.create_snapshot().unwrap();
        assert_eq!(snapshot.len(), recording.snapshot_len().unwrap());

        let mut resumed = ram_settings(10).compile_texts(&sources).unwrap();
        assert_eq!(resumed.restore_snapshot(&snapshot).unwrap(), "00");
        assert_eq!(resumed.tick_count(), 1);
        assert_eq!(resumed.applied_input(), [false, true, false, true]);
        assert_eq!(resumed.vram_page("screen", 0, 4).as_deref(), Some("1000"));
        assert_eq!(resumed.tick("0000").unwrap(), "10");
        assert_eq!(resumed.tick_count(), 2);
        resumed.reset();
        assert_eq!(resumed.tick_count(), 0);
        assert_eq!(resumed.generation, 1);

        let mut different_period = ram_settings(20).compile_texts(&sources).unwrap();
        let before = different_period.create_snapshot().unwrap();
        assert!(different_period.restore_snapshot(&snapshot).is_err());
        assert_eq!(different_period.create_snapshot().unwrap(), before);
    }
}
